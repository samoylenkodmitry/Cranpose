use cranpose_app_shell::PointerSource;
use cranpose_ui::PointerButton;
use winit::event::{
    ButtonSource, MouseButton, PointerSource as WinitPointerSource, TabletToolButton,
};

pub(crate) fn pointer_source_from_winit(source: &WinitPointerSource) -> PointerSource {
    match source {
        WinitPointerSource::Mouse => PointerSource::Mouse,
        WinitPointerSource::Touch { .. } => PointerSource::Touch,
        WinitPointerSource::TabletTool { .. } => PointerSource::Stylus,
        _ => PointerSource::Unknown,
    }
}

pub(crate) fn pointer_source_from_button(button: &ButtonSource) -> PointerSource {
    match button {
        ButtonSource::Mouse(_) => PointerSource::Mouse,
        ButtonSource::Touch { .. } => PointerSource::Touch,
        ButtonSource::TabletTool { .. } => PointerSource::Stylus,
        _ => PointerSource::Unknown,
    }
}

fn is_primary_tablet_button(button: TabletToolButton) -> bool {
    button == TabletToolButton::Contact
}

pub(crate) fn is_primary_pointer_button(button: &ButtonSource) -> bool {
    match button {
        ButtonSource::Mouse(button) => *button == MouseButton::Left,
        ButtonSource::Touch { .. } => true,
        ButtonSource::TabletTool { button, .. } => is_primary_tablet_button(*button),
        _ => false,
    }
}

/// The button of the framework that a mouse button other than the left one
/// is, or `None` for the left button, which presses as the primary pointer,
/// and for a button the framework has no name for.
pub(crate) fn other_mouse_button(button: &ButtonSource) -> Option<PointerButton> {
    match button {
        ButtonSource::Mouse(MouseButton::Right) => Some(PointerButton::Secondary),
        ButtonSource::Mouse(MouseButton::Middle) => Some(PointerButton::Middle),
        ButtonSource::Mouse(MouseButton::Back) => Some(PointerButton::Back),
        ButtonSource::Mouse(MouseButton::Forward) => Some(PointerButton::Forward),
        _ => None,
    }
}

#[cfg(test)]
#[path = "tests/winit_pointer_tests.rs"]
mod tests;
