//! The finger a winit pointer event belongs to, for
//! [`TouchRouter`](cranpose_app_shell::TouchRouter); `None` for a mouse.

use winit::event::{ButtonSource, FingerId, PointerKind, PointerSource as WinitPointerSource};

pub(crate) fn button_finger(button: &ButtonSource) -> Option<FingerId> {
    match button {
        ButtonSource::Touch { finger_id, .. } => Some(*finger_id),
        _ => None,
    }
}

pub(crate) fn source_finger(source: &WinitPointerSource) -> Option<FingerId> {
    match source {
        WinitPointerSource::Touch { finger_id, .. } => Some(*finger_id),
        _ => None,
    }
}

pub(crate) fn kind_finger(kind: &PointerKind) -> Option<FingerId> {
    match kind {
        PointerKind::Touch(finger_id) => Some(*finger_id),
        _ => None,
    }
}
