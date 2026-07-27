//! Maps winit's per-finger [`FingerId`]s onto the shell's pointer id space.
//!
//! winit reports every finger of a multi-touch interaction as its own opaque
//! [`FingerId`]. The shell has a single *primary* pointer (id `0`, driven by
//! `set_cursor`/`pointer_pressed`/`pointer_released_at_position`) plus the
//! `secondary_pointer_*` family for the additional fingers of a gesture. Without
//! this mapping every finger collapses onto pointer `0`: `TransformGesture`
//! never sees two pointers (so pinch is unreachable) and the scroll modifier's
//! `event.id != 0` arbitration cannot tell a second finger from the first.
//!
//! The routing mirrors the Android ingress (`android::shell_pointer_id` and
//! `push_pending_inputs_from_android_event`), which is the contract
//! `AppShell::secondary_pointer_*` was written against:
//!
//! * the finger that starts the gesture drives the primary pointer,
//! * every other finger gets a stable non-zero id for as long as it is down,
//! * when the primary finger lifts the whole gesture ends — the remaining
//!   secondaries are released first, then the primary — and the next finger
//!   down starts a fresh gesture.
//!
//! Android can quote each finger's own coordinates when it closes the
//! secondaries left over from a primary lift, because its `MotionEvent` carries
//! the whole pointer set. winit delivers one finger per event, so the leftover
//! secondaries are released at the lifting finger's position. Nothing consumes
//! the position of a secondary `Up` (`TransformGesture` only drops the entry,
//! and the scroll/zoom modifiers ignore non-zero ids there), so this costs
//! nothing.

use winit::event::{ButtonSource, FingerId, PointerKind, PointerSource as WinitPointerSource};

/// Which shell pointer a winit pointer event drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TouchRoute {
    /// The shell's primary pointer (id `0`).
    Primary,
    /// An additional finger of a multi-touch gesture, with a stable non-zero id
    /// for `AppShell::secondary_pointer_*`.
    Secondary(u64),
    /// A finger the router is not tracking: it either arrived while another
    /// gesture owned the primary pointer and has since been retired, or its
    /// press was never seen. Dropping it keeps stray fingers from hijacking the
    /// primary pointer mid-gesture.
    Untracked,
}

/// What a finger lifting has to dispatch, in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TouchRelease {
    /// Secondary pointer ids to release first, in ascending id order.
    pub(crate) secondaries: Vec<u64>,
    /// Whether the shell's primary pointer is released afterwards.
    pub(crate) releases_primary: bool,
}

/// What a `PointerLeft` has to dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TouchLeave {
    /// Cancel the whole gesture (`AppShell::cancel_gesture`).
    CancelGesture,
    /// Release just this secondary pointer; the primary gesture continues.
    ReleaseSecondary(u64),
    /// Nothing to do — this finger's stream already ended.
    Ignore,
}

/// Tracks the fingers of the in-flight gesture and hands out shell pointer ids.
#[derive(Debug, Default)]
pub(crate) struct TouchPointerRouter {
    /// The finger that started the gesture; drives the primary pointer.
    primary: Option<FingerId>,
    /// Additional fingers currently down, with their assigned shell ids.
    secondaries: Vec<(FingerId, u64)>,
    /// Next id to hand out. Reset with the gesture so ids stay small.
    next_secondary_id: u64,
}

/// The finger carried by a pointer event, when it is a touch.
fn button_finger(button: &ButtonSource) -> Option<FingerId> {
    match button {
        ButtonSource::Touch { finger_id, .. } => Some(*finger_id),
        _ => None,
    }
}

fn source_finger(source: &WinitPointerSource) -> Option<FingerId> {
    match source {
        WinitPointerSource::Touch { finger_id, .. } => Some(*finger_id),
        _ => None,
    }
}

fn kind_finger(kind: &PointerKind) -> Option<FingerId> {
    match kind {
        PointerKind::Touch(finger_id) => Some(*finger_id),
        _ => None,
    }
}

impl TouchPointerRouter {
    /// True while no finger of a gesture is tracked.
    fn is_idle(&self) -> bool {
        self.primary.is_none() && self.secondaries.is_empty()
    }

    fn secondary_id(&self, finger: FingerId) -> Option<u64> {
        self.secondaries
            .iter()
            .find(|(tracked, _)| *tracked == finger)
            .map(|(_, id)| *id)
    }

    fn end_gesture(&mut self) {
        self.primary = None;
        self.secondaries.clear();
        self.next_secondary_id = 0;
    }

    /// Routes a pressed `PointerButton`.
    ///
    /// Non-touch buttons (mouse, stylus contact) always drive the primary
    /// pointer — they are single-pointer devices as far as the shell is
    /// concerned.
    pub(crate) fn press(&mut self, button: &ButtonSource) -> TouchRoute {
        let Some(finger) = button_finger(button) else {
            return TouchRoute::Primary;
        };

        match self.primary {
            None => {
                self.primary = Some(finger);
                TouchRoute::Primary
            }
            Some(primary) if primary == finger => TouchRoute::Primary,
            Some(_) => {
                if let Some(id) = self.secondary_id(finger) {
                    return TouchRoute::Secondary(id);
                }
                self.next_secondary_id += 1;
                let id = self.next_secondary_id;
                self.secondaries.push((finger, id));
                TouchRoute::Secondary(id)
            }
        }
    }

    /// Routes a `PointerMoved`. Fingers that are not tracked are dropped rather
    /// than moved onto the primary pointer.
    pub(crate) fn moved(&self, source: &WinitPointerSource) -> TouchRoute {
        let Some(finger) = source_finger(source) else {
            return TouchRoute::Primary;
        };
        if self.primary == Some(finger) {
            return TouchRoute::Primary;
        }
        match self.secondary_id(finger) {
            Some(id) => TouchRoute::Secondary(id),
            None => TouchRoute::Untracked,
        }
    }

    /// Routes a released `PointerButton`.
    pub(crate) fn release(&mut self, button: &ButtonSource) -> TouchRelease {
        let primary_only = TouchRelease {
            secondaries: Vec::new(),
            releases_primary: true,
        };
        let Some(finger) = button_finger(button) else {
            return primary_only;
        };

        if self.primary == Some(finger) {
            // The gesture is anchored to the primary pointer, so its lift ends
            // the whole gesture: close the fingers still down, then the primary.
            let mut secondaries: Vec<u64> = self.secondaries.iter().map(|(_, id)| *id).collect();
            secondaries.sort_unstable();
            self.end_gesture();
            return TouchRelease {
                secondaries,
                releases_primary: true,
            };
        }

        if let Some(id) = self.secondary_id(finger) {
            self.secondaries.retain(|(tracked, _)| *tracked != finger);
            return TouchRelease {
                secondaries: vec![id],
                releases_primary: false,
            };
        }

        // An untracked finger. While a gesture is live it must not close it —
        // that is the fingers-left-over-after-a-primary-lift case. With nothing
        // tracked this is the ordinary trailing release, which still ends the
        // shell's pointer stream (matching the Android `ACTION_UP` fallthrough).
        if self.is_idle() {
            primary_only
        } else {
            TouchRelease::default()
        }
    }

    /// Routes a `PointerLeft`.
    ///
    /// iOS emits one of these per finger — after the matching release for a
    /// normal lift, and *instead* of a release when the system cancels the
    /// touch. Cancelling the whole gesture on every one of them (the previous
    /// behaviour) tears down the primary gesture the moment a second finger
    /// leaves, which is exactly what a pinch does.
    pub(crate) fn left(&mut self, kind: &PointerKind) -> TouchLeave {
        let Some(finger) = kind_finger(kind) else {
            self.end_gesture();
            return TouchLeave::CancelGesture;
        };

        if self.primary == Some(finger) {
            self.end_gesture();
            return TouchLeave::CancelGesture;
        }

        if let Some(id) = self.secondary_id(finger) {
            self.secondaries.retain(|(tracked, _)| *tracked != finger);
            return TouchLeave::ReleaseSecondary(id);
        }

        // Untracked: the trailing `PointerLeft` of a finger whose release was
        // already dispatched. Only forward it as a cancel when no gesture is in
        // flight, preserving the single-finger behaviour this replaced.
        if self.is_idle() {
            TouchLeave::CancelGesture
        } else {
            TouchLeave::Ignore
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::event::MouseButton;

    fn finger(raw: usize) -> FingerId {
        FingerId::from_raw(raw)
    }

    fn touch_button(raw: usize) -> ButtonSource {
        ButtonSource::Touch {
            finger_id: finger(raw),
            force: None,
        }
    }

    fn touch_source(raw: usize) -> WinitPointerSource {
        WinitPointerSource::Touch {
            finger_id: finger(raw),
            force: None,
        }
    }

    /// The regression: iOS discarded the `FingerId`, so a second finger's Down
    /// reached the shell as another primary press. Two Downs with DIFFERENT
    /// finger ids must produce a primary AND a secondary pointer.
    #[test]
    fn two_touch_downs_with_different_finger_ids_produce_a_primary_and_a_secondary() {
        let mut router = TouchPointerRouter::default();

        assert_eq!(router.press(&touch_button(11)), TouchRoute::Primary);
        assert_eq!(router.press(&touch_button(22)), TouchRoute::Secondary(1));
    }

    #[test]
    fn a_third_finger_gets_its_own_stable_non_zero_id() {
        let mut router = TouchPointerRouter::default();

        router.press(&touch_button(11));
        assert_eq!(router.press(&touch_button(22)), TouchRoute::Secondary(1));
        assert_eq!(router.press(&touch_button(33)), TouchRoute::Secondary(2));
        // Ids stay put for the life of the gesture.
        assert_eq!(
            router.moved(&touch_source(22)),
            TouchRoute::Secondary(1),
            "a finger must keep its id across moves"
        );
        assert_eq!(router.moved(&touch_source(33)), TouchRoute::Secondary(2));
    }

    #[test]
    fn moves_route_each_finger_to_its_own_pointer() {
        let mut router = TouchPointerRouter::default();
        router.press(&touch_button(11));
        router.press(&touch_button(22));

        assert_eq!(router.moved(&touch_source(11)), TouchRoute::Primary);
        assert_eq!(router.moved(&touch_source(22)), TouchRoute::Secondary(1));
    }

    #[test]
    fn moves_from_an_untracked_finger_do_not_steal_the_primary_pointer() {
        let mut router = TouchPointerRouter::default();
        router.press(&touch_button(11));

        assert_eq!(router.moved(&touch_source(99)), TouchRoute::Untracked);
    }

    #[test]
    fn lifting_a_secondary_finger_leaves_the_primary_gesture_running() {
        let mut router = TouchPointerRouter::default();
        router.press(&touch_button(11));
        router.press(&touch_button(22));

        assert_eq!(
            router.release(&touch_button(22)),
            TouchRelease {
                secondaries: vec![1],
                releases_primary: false,
            }
        );
        assert_eq!(router.moved(&touch_source(11)), TouchRoute::Primary);
    }

    #[test]
    fn lifting_the_primary_finger_closes_the_secondaries_first() {
        let mut router = TouchPointerRouter::default();
        router.press(&touch_button(11));
        router.press(&touch_button(22));
        router.press(&touch_button(33));

        assert_eq!(
            router.release(&touch_button(11)),
            TouchRelease {
                secondaries: vec![1, 2],
                releases_primary: true,
            }
        );
        // The gesture is over; the fingers still down are retired.
        assert_eq!(router.moved(&touch_source(22)), TouchRoute::Untracked);
    }

    #[test]
    fn a_new_finger_after_the_gesture_ends_becomes_the_next_primary() {
        let mut router = TouchPointerRouter::default();
        router.press(&touch_button(11));
        router.release(&touch_button(11));

        assert_eq!(router.press(&touch_button(44)), TouchRoute::Primary);
        assert_eq!(
            router.press(&touch_button(55)),
            TouchRoute::Secondary(1),
            "secondary ids restart with each gesture"
        );
    }

    #[test]
    fn pointer_left_for_a_secondary_finger_does_not_cancel_the_gesture() {
        let mut router = TouchPointerRouter::default();
        router.press(&touch_button(11));
        router.press(&touch_button(22));

        assert_eq!(
            router.left(&PointerKind::Touch(finger(22))),
            TouchLeave::ReleaseSecondary(1)
        );
        assert_eq!(router.moved(&touch_source(11)), TouchRoute::Primary);
    }

    #[test]
    fn pointer_left_trailing_a_release_still_cancels_once_the_gesture_is_over() {
        let mut router = TouchPointerRouter::default();
        router.press(&touch_button(11));
        router.release(&touch_button(11));

        assert_eq!(
            router.left(&PointerKind::Touch(finger(11))),
            TouchLeave::CancelGesture,
            "the single-finger sequence must behave as it did before"
        );
    }

    #[test]
    fn pointer_left_trailing_a_secondary_release_is_ignored_mid_gesture() {
        let mut router = TouchPointerRouter::default();
        router.press(&touch_button(11));
        router.press(&touch_button(22));
        router.release(&touch_button(22));

        assert_eq!(
            router.left(&PointerKind::Touch(finger(22))),
            TouchLeave::Ignore
        );
    }

    #[test]
    fn pointer_left_for_the_primary_finger_cancels_the_gesture() {
        let mut router = TouchPointerRouter::default();
        router.press(&touch_button(11));
        router.press(&touch_button(22));

        assert_eq!(
            router.left(&PointerKind::Touch(finger(11))),
            TouchLeave::CancelGesture
        );
        assert!(router.is_idle());
    }

    #[test]
    fn mouse_input_always_drives_the_primary_pointer() {
        let mut router = TouchPointerRouter::default();

        let button = ButtonSource::Mouse(MouseButton::Left);
        assert_eq!(router.press(&button), TouchRoute::Primary);
        assert_eq!(
            router.moved(&WinitPointerSource::Mouse),
            TouchRoute::Primary
        );
        assert_eq!(
            router.release(&button),
            TouchRelease {
                secondaries: Vec::new(),
                releases_primary: true,
            }
        );
        assert_eq!(router.left(&PointerKind::Mouse), TouchLeave::CancelGesture);
    }
}
