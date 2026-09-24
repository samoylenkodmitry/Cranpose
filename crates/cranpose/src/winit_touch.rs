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
#[path = "tests/winit_touch_tests.rs"]
mod tests;
