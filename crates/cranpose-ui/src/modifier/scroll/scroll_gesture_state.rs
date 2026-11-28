// What the scroll's gesture holds between the events of a press: where it went down and moved last,
// whether it drags or overscrolls, its velocity and the animations that it started.

use cranpose_foundation::ScrollId;

use super::{FlingAnimation, GestureVelocity, Point, SettleAnimation, WheelSettleWatcher};

pub(super) struct ScrollGestureState {
    pub(super) disposed: bool,
    pub(super) drag_down_position: Option<Point>,

    pub(super) last_position: Option<Point>,

    pub(super) is_dragging: bool,

    /// Whether the press stopped a scroll animation the app started, so
    /// letting go settles even without a drag.
    pub(super) interrupted_animation: bool,

    pub(super) axis_locked_out: bool,

    pub(super) velocity: GestureVelocity,

    pub(super) fling_animation: Option<FlingAnimation>,

    pub(super) is_overscrolling: bool,

    pub(super) settle_animation: Option<SettleAnimation>,

    pub(super) wheel_settle_watcher: Option<WheelSettleWatcher>,

    // The running scroll of the target that the fling or the settle holds.
    pub(super) running_scroll: Option<ScrollId>,
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
    pub(super) fn is_active(&self) -> bool {
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
