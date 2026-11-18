use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{NodeId, current_runtime_handle, internal::FrameCallbackRegistration};
use cranpose_foundation::{
    DRAG_THRESHOLD, DelegatableNode, ModifierNode, ModifierNodeElement, NodeCapabilities,
    NodeState, PointerButton, PointerButtons, ScrollId, lazy::LazyListState,
};
use cranpose_ui_layout::Axis;

mod drag_gesture;
mod release_motion;
mod scroll_target;
use drag_gesture::{DragGesture, drag_gesture_input, nested_drag_gesture_input};
use scroll_target::{NestedTarget, ScrollTarget};

use super::{Modifier, Point, PointerEvent, PointerEventKind, inspector_metadata};
use crate::{
    bring_into_view_spec::current_bring_into_view_spec,
    draggable::{DraggableState, ScrollableState},
    fling_animation::{
        FlingAnimation, MIN_FLING_VELOCITY, SettleAnimation, SpringParams, fling_rest_position,
    },
    gesture_velocity::GestureVelocity,
    render_state::schedule_modifier_slices_repass,
    scroll::{
        ScrollElement, ScrollMotionContext, ScrollMotionContextKey, ScrollState,
        scroll_motion_context_for_key,
    },
    scroll_semantics::lazy_scroll_semantics,
};

/// Returns the velocity of the last fling that started in the current app context, in dp per
/// second.
#[cfg(feature = "test-helpers")]
pub fn last_fling_velocity() -> f32 {
    crate::render_state::debug_last_fling_velocity()
}

/// Sets the value that [`last_fling_velocity`] returns to zero.
#[cfg(feature = "test-helpers")]
pub fn reset_last_fling_velocity() {
    crate::render_state::debug_reset_last_fling_velocity();
}

#[inline]
fn set_last_fling_velocity(velocity: f32) {
    crate::render_state::record_last_fling_velocity(velocity);
}

struct ScrollGestureState {
    disposed: bool,
    drag_down_position: Option<Point>,

    last_position: Option<Point>,

    is_dragging: bool,

    /// Whether the press stopped a scroll animation the app started, so
    /// letting go settles even without a drag.
    interrupted_animation: bool,

    axis_locked_out: bool,

    velocity: GestureVelocity,

    fling_animation: Option<FlingAnimation>,

    is_overscrolling: bool,

    settle_animation: Option<SettleAnimation>,

    wheel_settle_watcher: Option<WheelSettleWatcher>,

    // The running scroll of the target that the fling or the settle holds.
    running_scroll: Option<ScrollId>,
}

impl Default for ScrollGestureState {
    fn default() -> Self {
        Self {
            disposed: false,
            drag_down_position: None,
            last_position: None,
            is_dragging: false,
            interrupted_animation: false,
            axis_locked_out: false,
            velocity: GestureVelocity::new(),
            fling_animation: None,
            is_overscrolling: false,
            settle_animation: None,
            wheel_settle_watcher: None,
            running_scroll: None,
        }
    }
}

impl ScrollGestureState {
    fn is_active(&self) -> bool {
        self.is_dragging
            || self.is_overscrolling
            || self
                .fling_animation
                .as_ref()
                .is_some_and(FlingAnimation::is_running)
            || self
                .settle_animation
                .as_ref()
                .is_some_and(SettleAnimation::is_running)
            || self
                .wheel_settle_watcher
                .as_ref()
                .is_some_and(|watcher| watcher.is_running.get())
    }
}

#[inline]
fn delta_along(from: Point, to: Point, axis: Axis) -> f32 {
    match axis {
        Axis::Horizontal => to.x - from.x,
        Axis::Vertical => to.y - from.y,
    }
}

const WHEEL_SETTLE_IDLE_NANOS: u64 = 180_000_000;

struct WheelSettleWatcher {
    is_running: Rc<Cell<bool>>,
    registration: Rc<RefCell<Option<FrameCallbackRegistration>>>,
}

impl WheelSettleWatcher {
    fn cancel(&self) {
        self.is_running.set(false);
        self.registration.borrow_mut().take();
    }
}

struct ScrollGestureDetector<S: ScrollTarget> {
    gesture_state: Rc<RefCell<ScrollGestureState>>,

    scroll_target: S,

    axis: Axis,

    reverse_scrolling: bool,

    overscroll: crate::scroll::OverscrollEffect,

    motion_context: ScrollMotionContext,
}

impl<S: ScrollTarget + 'static> ScrollGestureDetector<S> {
    fn new(
        gesture_state: Rc<RefCell<ScrollGestureState>>,
        scroll_target: S,
        axis: Axis,
        reverse_scrolling: bool,
        overscroll: crate::scroll::OverscrollEffect,
        motion_context: ScrollMotionContext,
    ) -> Self {
        Self {
            gesture_state,
            scroll_target,
            axis,
            reverse_scrolling,
            overscroll,
            motion_context,
        }
    }

    // A delta of the pointer as the scroll target takes it.
    fn toward_target(&self, pointer_delta: f32) -> f32 {
        if self.reverse_scrolling {
            -pointer_delta
        } else {
            pointer_delta
        }
    }

    fn along_axis(&self, point: Point) -> f32 {
        if self.axis.is_vertical() {
            point.y
        } else {
            point.x
        }
    }

    fn on_down(&self, event: &PointerEvent) -> bool {
        let position = event.position;
        let interrupted = self.scroll_target.stop_animation();
        let mut gs = self.gesture_state.borrow_mut();
        gs.interrupted_animation = interrupted;

        if let Some(fling) = gs.fling_animation.take() {
            fling.cancel();
        }
        if let Some(settle) = gs.settle_animation.take() {
            settle.cancel();
        }
        if let Some(watcher) = gs.wheel_settle_watcher.take() {
            watcher.cancel();
        }
        self.motion_context.set_active(false);

        gs.drag_down_position = Some(position);
        gs.last_position = Some(position);
        gs.is_dragging = false;
        gs.axis_locked_out = false;
        gs.is_overscrolling = self.overscroll.offset().abs() > 0.001;
        gs.velocity
            .begin(event, self.along_axis(event.global_position));

        false
    }

    fn on_move(&self, position: Point, buttons: PointerButtons, event: &PointerEvent) -> bool {
        let mut gs = self.gesture_state.borrow_mut();

        if !buttons.contains(PointerButton::Primary) && gs.drag_down_position.is_some() {
            if gs.is_dragging {
                self.scroll_target.set_dragging(false);
            }
            gs.drag_down_position = None;
            gs.last_position = None;
            gs.is_dragging = false;
            gs.axis_locked_out = false;
            gs.velocity.clear();
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

        let incremental_delta = delta_along(last_pos, position, self.axis);

        let mut edge_candidate = false;
        if !gs.is_dragging && !gs.axis_locked_out {
            let signed_main_delta = delta_along(down_pos, position, self.axis);
            let main_delta = signed_main_delta.abs();
            let cross_delta = delta_along(down_pos, position, self.axis.cross_axis()).abs();
            if main_delta > DRAG_THRESHOLD && main_delta >= cross_delta {
                if self
                    .scroll_target
                    .can_consume(self.toward_target(signed_main_delta))
                {
                    gs.is_dragging = true;
                    self.scroll_target.set_dragging(true);
                    self.motion_context.set_active(true);
                } else if self.scroll_target.can_scroll() {
                    edge_candidate = true;
                }
            } else if cross_delta > DRAG_THRESHOLD && cross_delta > main_delta {
                gs.axis_locked_out = true;
            }
        }

        gs.last_position = Some(position);

        // In the window: an element that follows the finger barely moves under it.
        gs.velocity
            .add(event, self.along_axis(event.global_position));

        if gs.is_dragging {
            drop(gs);
            let delta = self.toward_target(incremental_delta);
            let overscroll = self.overscroll.clone();
            overscroll.apply_to_scroll(delta, |delta| self.scroll_target.apply_delta(delta));
            self.scroll_target.invalidate();
            true
        } else if gs.is_overscrolling {
            drop(gs);
            let delta = self.toward_target(incremental_delta);
            self.apply_overscroll_delta(delta)
        } else if edge_candidate {
            drop(gs);
            let delta = self.toward_target(incremental_delta);
            let detector = self.clone_for_watcher();
            event.defer_post_dispatch_action(move || detector.apply_overscroll_candidate(delta));
            false
        } else {
            false
        }
    }

    fn apply_overscroll_delta(&self, delta: f32) -> bool {
        self.motion_context.set_active(true);
        let overscroll = self.overscroll.clone();
        let target_consumed = Cell::new(0.0);
        let before_overscroll = overscroll.offset();
        overscroll.apply_to_scroll(delta, |delta| {
            let consumed = self.scroll_target.apply_delta(delta);
            target_consumed.set(consumed);
            consumed
        });
        self.scroll_target.invalidate();
        let consumed = target_consumed.get().abs() > 0.001
            || (overscroll.offset() - before_overscroll).abs() > 0.001;
        if !consumed {
            self.motion_context.set_active(false);
        }
        consumed
    }

    fn apply_overscroll_candidate(&self, delta: f32) -> bool {
        if self.gesture_state.borrow().disposed {
            return false;
        }
        let consumed = self.apply_overscroll_delta(delta);
        if consumed {
            self.gesture_state.borrow_mut().is_overscrolling = true;
        }
        consumed
    }

    fn on_scroll(&self, axis_delta: f32, event: &PointerEvent) -> bool {
        if axis_delta.abs() <= f32::EPSILON {
            return false;
        }

        let delta = self.toward_target(axis_delta);
        if !self.scroll_target.can_consume(delta) && self.scroll_target.can_scroll() {
            let detector = self.clone_for_watcher();
            event.defer_post_dispatch_action(move || detector.apply_wheel_delta(delta));
            return false;
        }
        self.apply_wheel_delta(delta)
    }

    fn apply_wheel_delta(&self, delta: f32) -> bool {
        {
            let mut gs = self.gesture_state.borrow_mut();
            if gs.disposed {
                return false;
            }
            if let Some(fling) = gs.fling_animation.take() {
                fling.cancel();
            }
            if let Some(settle) = gs.settle_animation.take() {
                settle.cancel();
            }
            gs.drag_down_position = None;
            gs.last_position = None;
            gs.is_dragging = false;
            gs.axis_locked_out = false;
            gs.velocity.clear();
        }

        self.motion_context.activate_for_current_frame();
        let overscroll = self.overscroll.clone();
        let consumed =
            overscroll.apply_to_scroll(delta, |delta| self.scroll_target.apply_wheel_delta(delta));
        let overscroll_active = self.overscroll.offset().abs() > 0.001;
        if consumed.abs() > 0.001 {
            self.scroll_target.invalidate();
        }
        if consumed.abs() > 0.001 || overscroll_active {
            self.ensure_wheel_settle_watcher();
            true
        } else {
            false
        }
    }

    fn ensure_wheel_settle_watcher(&self) {
        if self.scroll_target.settle_policy().is_none() && self.overscroll.offset().abs() <= 0.001 {
            return;
        }
        {
            let gs = self.gesture_state.borrow();
            if gs
                .wheel_settle_watcher
                .as_ref()
                .is_some_and(|watcher| watcher.is_running.get())
            {
                return;
            }
        }
        let Some(runtime) = current_runtime_handle() else {
            return;
        };

        let is_running = Rc::new(Cell::new(true));
        let registration = Rc::new(RefCell::new(None));

        struct WheelSettleLoop<S: ScrollTarget> {
            detector: ScrollGestureDetector<S>,
            gesture_state: Rc<RefCell<ScrollGestureState>>,
            frame_clock: cranpose_core::internal::FrameClock,
            is_running: Rc<Cell<bool>>,
            registration: Rc<RefCell<Option<FrameCallbackRegistration>>>,
            last_offset: Rc<Cell<f32>>,
            idle_nanos: Rc<Cell<u64>>,
            last_frame: Rc<Cell<Option<u64>>>,
        }

        impl<S: ScrollTarget + 'static> WheelSettleLoop<S> {
            fn next(&self) -> Self {
                Self {
                    detector: self.detector.clone_for_watcher(),
                    gesture_state: Rc::clone(&self.gesture_state),
                    frame_clock: self.frame_clock.clone(),
                    is_running: Rc::clone(&self.is_running),
                    registration: Rc::clone(&self.registration),
                    last_offset: Rc::clone(&self.last_offset),
                    idle_nanos: Rc::clone(&self.idle_nanos),
                    last_frame: Rc::clone(&self.last_frame),
                }
            }

            fn schedule(self) {
                let continuation = self.next();
                let registration_slot = Rc::clone(&self.registration);
                let new_registration = self.frame_clock.with_frame_nanos(move |frame_time_nanos| {
                    let this = &continuation;
                    if !this.is_running.get() {
                        return;
                    }
                    {
                        let gs = this.gesture_state.borrow();
                        let animating = gs.is_dragging
                            || gs
                                .fling_animation
                                .as_ref()
                                .is_some_and(FlingAnimation::is_running)
                            || gs
                                .settle_animation
                                .as_ref()
                                .is_some_and(SettleAnimation::is_running);
                        if animating {
                            this.is_running.set(false);
                            return;
                        }
                    }

                    let offset = this.detector.scroll_target.current_offset();
                    let dt = this
                        .last_frame
                        .get()
                        .map_or(0, |last| frame_time_nanos.saturating_sub(last));
                    this.last_frame.set(Some(frame_time_nanos));
                    if (offset - this.last_offset.get()).abs() > 0.01 {
                        this.last_offset.set(offset);
                        this.idle_nanos.set(0);
                    } else {
                        this.idle_nanos.set(this.idle_nanos.get() + dt);
                    }

                    if this.idle_nanos.get() >= WHEEL_SETTLE_IDLE_NANOS {
                        this.is_running.set(false);
                        if this.detector.overscroll.offset().abs() > 0.001 {
                            this.detector.start_overscroll_settle(0.0);
                            return;
                        }
                        if let Some(policy) = this.detector.scroll_target.settle_policy() {
                            let target = policy(offset, 0.0);
                            if (target - offset).abs() > 0.5 {
                                this.detector.start_settle_animation(target, 0.0);
                            }
                        }
                        return;
                    }

                    continuation.next().schedule();
                });
                *registration_slot.borrow_mut() = Some(new_registration);
            }
        }

        WheelSettleLoop {
            detector: self.clone_for_watcher(),
            gesture_state: Rc::clone(&self.gesture_state),
            frame_clock: runtime.frame_clock(),
            is_running: Rc::clone(&is_running),
            registration: Rc::clone(&registration),
            last_offset: Rc::new(Cell::new(self.scroll_target.current_offset())),
            idle_nanos: Rc::new(Cell::new(0u64)),
            last_frame: Rc::new(Cell::new(None::<u64>)),
        }
        .schedule();

        self.gesture_state.borrow_mut().wheel_settle_watcher = Some(WheelSettleWatcher {
            is_running,
            registration,
        });
    }

    fn clone_for_watcher(&self) -> Self {
        Self {
            gesture_state: Rc::clone(&self.gesture_state),
            scroll_target: self.scroll_target.clone(),
            axis: self.axis,
            reverse_scrolling: self.reverse_scrolling,
            overscroll: self.overscroll.clone(),
            motion_context: self.motion_context.clone(),
        }
    }
}

pub(crate) struct MotionContextAnimatedNode {
    state: NodeState,
    motion_context: ScrollMotionContext,
    invalidation_callback_id: Option<u64>,
    overscroll_callback_id: Option<u64>,
    node_id: Option<NodeId>,
}

impl MotionContextAnimatedNode {
    fn new(motion_context: ScrollMotionContext) -> Self {
        Self {
            state: NodeState::new(),
            motion_context,
            invalidation_callback_id: None,
            overscroll_callback_id: None,
            node_id: None,
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        self.motion_context.is_active()
    }

    fn attach_callbacks(&mut self) {
        if let Some(node_id) = self.node_id {
            self.invalidation_callback_id = Some(self.motion_context.add_invalidate_callback(
                Box::new(move || schedule_modifier_slices_repass(node_id)),
            ));
            self.overscroll_callback_id = Some(
                self.motion_context
                    .overscroll()
                    .add_invalidate_callback(Box::new(move || {
                        crate::schedule_measure_repass(node_id);
                        schedule_modifier_slices_repass(node_id);
                    })),
            );
        }
    }

    fn detach_callbacks(&mut self) {
        if let Some(id) = self.invalidation_callback_id.take() {
            self.motion_context.remove_invalidate_callback(id);
        }
        if let Some(id) = self.overscroll_callback_id.take() {
            self.motion_context
                .overscroll()
                .remove_invalidate_callback(id);
        }
    }
}

pub(crate) struct TranslatedContentContextNode {
    state: NodeState,
    identity: usize,
    offset_source: TranslatedContentOffsetSource,
    overscroll: crate::scroll::OverscrollEffect,
    overscroll_callback_id: Option<u64>,
    node_id: Option<NodeId>,
}

impl TranslatedContentContextNode {
    fn new(
        identity: usize,
        offset_source: TranslatedContentOffsetSource,
        overscroll: crate::scroll::OverscrollEffect,
    ) -> Self {
        Self {
            state: NodeState::new(),
            identity,
            offset_source,
            overscroll,
            overscroll_callback_id: None,
            node_id: None,
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        true
    }

    pub(crate) fn identity(&self) -> usize {
        self.identity
    }

    pub(crate) fn content_offset_reader(&self) -> Option<Rc<dyn Fn() -> Point>> {
        self.offset_source
            .content_offset_reader(self.overscroll.clone())
    }

    fn attach_callback(&mut self) {
        if let Some(node_id) = self.node_id {
            self.overscroll_callback_id =
                Some(self.overscroll.add_invalidate_callback(Box::new(move || {
                    schedule_modifier_slices_repass(node_id);
                })));
        }
    }

    fn detach_callback(&mut self) {
        if let Some(id) = self.overscroll_callback_id.take() {
            self.overscroll.remove_invalidate_callback(id);
        }
    }
}

impl DelegatableNode for TranslatedContentContextNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for TranslatedContentContextNode {
    fn on_attach(&mut self, context: &mut dyn cranpose_foundation::ModifierNodeContext) {
        self.node_id = context.node_id();
        self.attach_callback();
    }

    fn on_detach(&mut self) {
        self.detach_callback();
        self.node_id = None;
    }
}

impl DelegatableNode for MotionContextAnimatedNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for MotionContextAnimatedNode {
    fn on_attach(&mut self, context: &mut dyn cranpose_foundation::ModifierNodeContext) {
        self.node_id = context.node_id();
        self.attach_callbacks();
    }

    fn on_detach(&mut self) {
        self.detach_callbacks();
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
        node.detach_callbacks();
        node.motion_context = self.motion_context.clone();
        node.attach_callbacks();
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
        axis: Axis,
        reverse_scrolling: bool,
    },
}

impl TranslatedContentOffsetSource {
    fn content_offset_reader(
        &self,
        overscroll: crate::scroll::OverscrollEffect,
    ) -> Option<Rc<dyn Fn() -> Point>> {
        match self {
            Self::LayoutContentOffset => None,
            Self::LazyList { state, axis, .. } => Some(Rc::new(lazy_list_content_offset_reader(
                *state, *axis, overscroll,
            ))),
        }
    }

    fn axis(&self) -> Option<Axis> {
        match self {
            Self::LayoutContentOffset => None,
            Self::LazyList { axis, .. } => Some(*axis),
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

fn lazy_list_content_offset_reader(
    state: LazyListState,
    axis: Axis,
    overscroll: crate::scroll::OverscrollEffect,
) -> impl Fn() -> Point {
    move || {
        let Some((viewport, main_offset)) = state
            .with_layout_info(|info| {
                (!info.visible_items_info.is_empty())
                    .then_some((info.viewport_size, info.snap_anchor_offset))
            })
            .flatten()
        else {
            return Point::default();
        };
        overscroll.set_dimension(viewport);
        let bounce = overscroll.offset();
        if axis.is_vertical() {
            Point::new(0.0, main_offset + bounce)
        } else {
            Point::new(main_offset + bounce, 0.0)
        }
    }
}

#[derive(Clone)]
struct TranslatedContentContextElement {
    identity: usize,
    offset_source: TranslatedContentOffsetSource,
    overscroll: crate::scroll::OverscrollEffect,
}

impl TranslatedContentContextElement {
    fn new(
        identity: usize,
        offset_source: TranslatedContentOffsetSource,
        overscroll: crate::scroll::OverscrollEffect,
    ) -> Self {
        Self {
            identity,
            offset_source,
            overscroll,
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
            && self.overscroll.ptr_eq(&other.overscroll)
            && self.offset_source.axis() == other.offset_source.axis()
            && self.offset_source.reverse_scrolling() == other.offset_source.reverse_scrolling()
    }
}

impl Eq for TranslatedContentContextElement {}

impl std::hash::Hash for TranslatedContentContextElement {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.identity.hash(state);
        self.offset_source.axis().hash(state);
        self.offset_source.reverse_scrolling().hash(state);
    }
}

impl ModifierNodeElement for TranslatedContentContextElement {
    type Node = TranslatedContentContextNode;

    fn create(&self) -> Self::Node {
        TranslatedContentContextNode::new(
            self.identity,
            self.offset_source.clone(),
            self.overscroll.clone(),
        )
    }

    fn update(&self, node: &mut Self::Node) {
        node.identity = self.identity;
        node.offset_source = self.offset_source.clone();
        if !node.overscroll.ptr_eq(&self.overscroll) {
            node.detach_callback();
            node.overscroll = self.overscroll.clone();
            node.attach_callback();
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}

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
        self.then(scroll_impl(
            state,
            Axis::Horizontal,
            reverse_scrolling,
            None,
            (0.0, 0.0),
        ))
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
        self.then(scroll_impl(
            state,
            Axis::Vertical,
            reverse_scrolling,
            None,
            (0.0, 0.0),
        ))
    }

    /// Adds a vertical scroll container with content padding in logical pixels.
    ///
    /// The padding scrolls with the content. Keyboard and accessibility focus
    /// keep controls inside the padded viewport, clear of overlaid system UI.
    pub fn vertical_scroll_with_content_padding(
        self,
        state: ScrollState,
        reverse_scrolling: bool,
        padding: cranpose_ui_graphics::EdgeInsets,
    ) -> Self {
        let density = crate::density();
        self.then(scroll_impl(
            state,
            Axis::Vertical,
            reverse_scrolling,
            None,
            (density.dp(padding.top), density.dp(padding.bottom)),
        ))
        .window_insets_padding(padding)
    }

    /// Adds a horizontal scroll container with content padding in logical pixels.
    ///
    /// The padding scrolls with the content. Keyboard and accessibility focus
    /// keep controls inside the padded viewport. Insets use physical edges.
    pub fn horizontal_scroll_with_content_padding(
        self,
        state: ScrollState,
        reverse_scrolling: bool,
        padding: cranpose_ui_graphics::EdgeInsets,
    ) -> Self {
        let density = crate::density();
        self.then(scroll_impl(
            state,
            Axis::Horizontal,
            reverse_scrolling,
            None,
            (density.dp(padding.left), density.dp(padding.right)),
        ))
        .window_insets_padding(padding)
    }

    /// A horizontal scroll that never claims a pointer gesture.
    ///
    /// The position still moves through [`ScrollState`], but drags and wheel
    /// events pass straight through, so an owner can run its own
    /// `pointer_input` on the same row -- a drag-to-reorder list, say --
    /// without the two competing for the gesture.
    pub fn horizontal_scroll_without_gestures(
        self,
        state: ScrollState,
        reverse_scrolling: bool,
    ) -> Self {
        self.then(scroll_impl(
            state,
            Axis::Horizontal,
            reverse_scrolling,
            Some(Rc::new(|| false)),
            (0.0, 0.0),
        ))
    }

    /// A vertical scroll that never claims a pointer gesture.
    ///
    /// The vertical counterpart of [`Modifier::horizontal_scroll_without_gestures`].
    pub fn vertical_scroll_without_gestures(
        self,
        state: ScrollState,
        reverse_scrolling: bool,
    ) -> Self {
        self.then(scroll_impl(
            state,
            Axis::Vertical,
            reverse_scrolling,
            Some(Rc::new(|| false)),
            (0.0, 0.0),
        ))
    }

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
        let motion_context = scroll_motion_context_for_key(ScrollMotionContextKey::LazyList {
            state_identity: state.inner_ptr() as usize,
            axis: Axis::Vertical,
            reverse_scrolling,
        });
        self.lazy_vertical_scroll_with_context(state, reverse_scrolling, (0.0, 0.0), motion_context)
    }

    pub(crate) fn lazy_vertical_scroll_with_context(
        self,
        state: LazyListState,
        reverse_scrolling: bool,
        content_padding: (f32, f32),
        motion_context: ScrollMotionContext,
    ) -> Self {
        self.then(lazy_scroll_impl(
            state,
            Axis::Vertical,
            reverse_scrolling,
            content_padding,
            motion_context,
        ))
    }

    pub(crate) fn lazy_horizontal_scroll_with_context(
        self,
        state: LazyListState,
        reverse_scrolling: bool,
        content_padding: (f32, f32),
        motion_context: ScrollMotionContext,
    ) -> Self {
        self.then(lazy_scroll_impl(
            state,
            Axis::Horizontal,
            reverse_scrolling,
            content_padding,
            motion_context,
        ))
    }

    /// Drags along `axis`, reporting each delta to `state`.
    ///
    /// The gesture is the one the scroll containers run — touch slop before a
    /// drag begins, axis locking so a mostly-vertical drag never steals a
    /// horizontal one, the primary pointer only, and yielding to whoever
    /// consumed the event first — so a dragged control and a scrolled list feel
    /// the same under a finger. Unlike a scroll, a release does not fling: a
    /// control the user placed stays where it was let go.
    ///
    /// Deltas are logical pixels, positive to the right for [`Axis::Horizontal`]
    /// and downwards for [`Axis::Vertical`].
    pub fn draggable(self, axis: Axis, state: DraggableState) -> Self {
        self.then(draggable_impl(axis, state, None))
    }

    /// Scrolls content with custom placement along `axis`.
    ///
    /// Supplies drag, wheel and inertial scrolling without translating or
    /// clipping layout. A new pointer press interrupts the fling. The state's
    /// consumption handler stops it when the content reaches a boundary.
    pub fn scrollable(self, axis: Axis, state: ScrollableState) -> Self {
        let identity = state.identity();
        let motion_context = scroll_motion_context_for_key(ScrollMotionContextKey::Draggable {
            state_identity: identity,
            axis,
        });
        self.then(Self::with_element(MotionContextAnimatedElement::new(
            motion_context.clone(),
        )))
        .then(nested_drag_gesture_input(
            (identity, axis),
            DragGesture {
                target: state,
                axis,
                reverse_input: false,
                motion_context,
                guard: None,
            },
        ))
    }

    /// Drags along `axis` only while `guard` says so.
    ///
    /// The guard is asked per event, so a control can stop accepting a drag
    /// half way through one and the gesture ends rather than being applied to
    /// a control that has since been disabled.
    pub fn draggable_guarded(
        self,
        axis: Axis,
        state: DraggableState,
        guard: impl Fn() -> bool + 'static,
    ) -> Self {
        self.then(draggable_impl(axis, state, Some(Rc::new(guard))))
    }
}

fn scroll_impl(
    state: ScrollState,
    axis: Axis,
    reverse_scrolling: bool,
    guard: Option<Rc<dyn Fn() -> bool>>,
    content_padding: (f32, f32),
) -> Modifier {
    let motion_context = scroll_motion_context_for_key(ScrollMotionContextKey::ScrollState {
        state_id: state.id(),
        axis,
        reverse_scrolling,
    });
    motion_context
        .observe_activity(|| Box::new(move |active| state.set_scroll_in_progress(active)));

    let pointer_input = nested_drag_gesture_input(
        (state.id(), axis),
        DragGesture {
            target: state,
            axis,
            reverse_input: false,
            motion_context: motion_context.clone(),
            guard,
        },
    );

    let overscroll = motion_context.overscroll();
    let element = ScrollElement::new(state, overscroll.clone(), axis, reverse_scrolling);
    let layout_modifier =
        Modifier::with_element(element).with_inspector_metadata(inspector_metadata(
            if axis.is_vertical() {
                "verticalScroll"
            } else {
                "horizontalScroll"
            },
            move |info| {
                info.add_property("isVertical", axis.is_vertical().to_string());
                info.add_property("reverseScrolling", reverse_scrolling.to_string());
            },
        ));
    let motion_modifier = Modifier::with_element(MotionContextAnimatedElement::new(motion_context));
    let translated_content_modifier = Modifier::with_element(TranslatedContentContextElement::new(
        state.id() as usize,
        TranslatedContentOffsetSource::LayoutContentOffset,
        overscroll,
    ));

    // The clip wraps the scroll layout, as Compose's `scrollingContainer`
    // clips before it scrolls: it cuts at the viewport, not at the content
    // the layout moves.
    pointer_input
        .clip_to_bounds()
        .then(motion_modifier)
        .then(translated_content_modifier)
        .then(layout_modifier)
        .semantics(scroll_semantics(
            state,
            axis,
            reverse_scrolling,
            content_padding,
        ))
}

/// What a screen reader learns about a scroll container: how far it has
/// scrolled, whether it can go on, and how to page it. Compose's
/// `verticalScrollAxisRange` / `horizontalScrollAxisRange` and `scrollBy`.
/// Without this a reader's cursor stops at the last row it can see.
fn scroll_semantics(
    state: ScrollState,
    axis: Axis,
    reverse_scrolling: bool,
    content_padding: (f32, f32),
) -> impl Fn(&mut cranpose_foundation::SemanticsConfiguration) + 'static {
    let spec = current_bring_into_view_spec();
    move |config| {
        let range = cranpose_foundation::ScrollAxisRange::new(
            state.value(),
            state.max_value(),
            reverse_scrolling,
        )
        .with_content_padding(content_padding.0, content_padding.1)
        .with_bring_into_view_spec(spec);
        if axis.is_vertical() {
            config.vertical_scroll = Some(range);
        } else {
            config.horizontal_scroll = Some(range);
        }
        config.scroll_by = Some(cranpose_foundation::SemanticsScrollBy::new(
            move |dx, dy| {
                let delta = if axis.is_vertical() { dy } else { dx };
                state.dispatch_raw_delta(delta).abs() > f32::EPSILON
            },
        ));
    }
}

fn lazy_scroll_impl(
    state: LazyListState,
    axis: Axis,
    reverse_scrolling: bool,
    content_padding: (f32, f32),
    motion_context: ScrollMotionContext,
) -> Modifier {
    let list_state = state;
    let state_id = state.inner_ptr() as usize;
    let key = (state_id, axis, reverse_scrolling);
    motion_context
        .observe_activity(|| Box::new(move |active| state.set_gesture_scroll_active(active)));
    let overscroll = motion_context.overscroll();
    let translated_content_modifier = Modifier::with_element(TranslatedContentContextElement::new(
        state_id,
        TranslatedContentOffsetSource::LazyList {
            state,
            axis,
            reverse_scrolling,
        },
        overscroll,
    ));

    Modifier::with_element(MotionContextAnimatedElement::new(motion_context.clone()))
        .then(translated_content_modifier)
        .then(nested_drag_gesture_input(
            key,
            DragGesture {
                target: list_state,
                axis,
                reverse_input: reverse_scrolling,
                motion_context,
                guard: None,
            },
        ))
        .semantics(lazy_scroll_semantics(
            list_state,
            axis,
            reverse_scrolling,
            content_padding,
        ))
}

fn draggable_impl(
    axis: Axis,
    state: DraggableState,
    guard: Option<Rc<dyn Fn() -> bool>>,
) -> Modifier {
    let identity = state.identity();
    drag_gesture_input(
        (identity, axis),
        DragGesture {
            target: state,
            axis,
            reverse_input: false,
            motion_context: scroll_motion_context_for_key(ScrollMotionContextKey::Draggable {
                state_identity: identity,
                axis,
            }),
            guard,
        },
    )
}
