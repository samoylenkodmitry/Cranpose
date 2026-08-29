use winit::event::{ButtonSource, FingerId, PointerKind, PointerSource as WinitPointerSource};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TouchRoute {
    Primary,
    Secondary(u64),
    Untracked,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TouchRelease {
    pub(crate) secondaries: Vec<u64>,
    pub(crate) releases_primary: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TouchLeave {
    CancelGesture,
    ReleaseSecondary(u64),
    Ignore,
}

#[derive(Debug, Default)]
pub(crate) struct TouchPointerRouter {
    primary: Option<FingerId>,
    secondaries: Vec<(FingerId, u64)>,
    next_secondary_id: u64,
}

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

    pub(crate) fn release(&mut self, button: &ButtonSource) -> TouchRelease {
        let primary_only = TouchRelease {
            secondaries: Vec::new(),
            releases_primary: true,
        };
        let Some(finger) = button_finger(button) else {
            return primary_only;
        };

        if self.primary == Some(finger) {
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

        if self.is_idle() {
            primary_only
        } else {
            TouchRelease::default()
        }
    }

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

        if self.is_idle() {
            TouchLeave::CancelGesture
        } else {
            TouchLeave::Ignore
        }
    }
}

#[cfg(test)]
mod tests {
    use winit::event::MouseButton;

    use super::*;

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
