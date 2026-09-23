use cranpose_app_shell::{KeyCode, SurfaceMut};
use winit::keyboard::{Key, KeyCode as WinitKeyCode, NamedKey, PhysicalKey};

pub(crate) fn dispatch_keyboard_input<R>(
    surface: &mut SurfaceMut<'_, R>,
    current_modifiers: winit::keyboard::ModifiersState,
    event: winit::event::KeyEvent,
) where
    R: cranpose_render_common::Renderer,
    R::Error: std::fmt::Debug,
{
    use cranpose_app_shell::{KeyEvent, KeyEventType};
    use winit::event::ElementState;

    let event_type = match event.state {
        ElementState::Pressed => KeyEventType::KeyDown,
        ElementState::Released => KeyEventType::KeyUp,
    };
    let text = match &event.logical_key {
        Key::Character(s) => s.to_string(),
        _ => String::new(),
    };
    let key_code = app_key_code(event.physical_key, &event.logical_key);
    let key_event = KeyEvent::new(key_code, text, app_modifiers(current_modifiers), event_type);

    if key_code == KeyCode::D && event_type == KeyEventType::KeyDown {
        surface.log_debug_info();
    }

    surface.on_key_event(&key_event);
}

const NAMED_KEYS: [(NamedKey, KeyCode); 11] = [
    (NamedKey::ArrowUp, KeyCode::ArrowUp),
    (NamedKey::ArrowDown, KeyCode::ArrowDown),
    (NamedKey::ArrowLeft, KeyCode::ArrowLeft),
    (NamedKey::ArrowRight, KeyCode::ArrowRight),
    (NamedKey::Home, KeyCode::Home),
    (NamedKey::End, KeyCode::End),
    (NamedKey::Enter, KeyCode::Enter),
    (NamedKey::Tab, KeyCode::Tab),
    (NamedKey::Escape, KeyCode::Escape),
    (NamedKey::Backspace, KeyCode::Backspace),
    (NamedKey::Delete, KeyCode::Delete),
];

const PHYSICAL_KEYS: [(WinitKeyCode, KeyCode); 49] = [
    (WinitKeyCode::KeyA, KeyCode::A),
    (WinitKeyCode::KeyB, KeyCode::B),
    (WinitKeyCode::KeyC, KeyCode::C),
    (WinitKeyCode::KeyD, KeyCode::D),
    (WinitKeyCode::KeyE, KeyCode::E),
    (WinitKeyCode::KeyF, KeyCode::F),
    (WinitKeyCode::KeyG, KeyCode::G),
    (WinitKeyCode::KeyH, KeyCode::H),
    (WinitKeyCode::KeyI, KeyCode::I),
    (WinitKeyCode::KeyJ, KeyCode::J),
    (WinitKeyCode::KeyK, KeyCode::K),
    (WinitKeyCode::KeyL, KeyCode::L),
    (WinitKeyCode::KeyM, KeyCode::M),
    (WinitKeyCode::KeyN, KeyCode::N),
    (WinitKeyCode::KeyO, KeyCode::O),
    (WinitKeyCode::KeyP, KeyCode::P),
    (WinitKeyCode::KeyQ, KeyCode::Q),
    (WinitKeyCode::KeyR, KeyCode::R),
    (WinitKeyCode::KeyS, KeyCode::S),
    (WinitKeyCode::KeyT, KeyCode::T),
    (WinitKeyCode::KeyU, KeyCode::U),
    (WinitKeyCode::KeyV, KeyCode::V),
    (WinitKeyCode::KeyW, KeyCode::W),
    (WinitKeyCode::KeyX, KeyCode::X),
    (WinitKeyCode::KeyY, KeyCode::Y),
    (WinitKeyCode::KeyZ, KeyCode::Z),
    (WinitKeyCode::Digit0, KeyCode::Digit0),
    (WinitKeyCode::Digit1, KeyCode::Digit1),
    (WinitKeyCode::Digit2, KeyCode::Digit2),
    (WinitKeyCode::Digit3, KeyCode::Digit3),
    (WinitKeyCode::Digit4, KeyCode::Digit4),
    (WinitKeyCode::Digit5, KeyCode::Digit5),
    (WinitKeyCode::Digit6, KeyCode::Digit6),
    (WinitKeyCode::Digit7, KeyCode::Digit7),
    (WinitKeyCode::Digit8, KeyCode::Digit8),
    (WinitKeyCode::Digit9, KeyCode::Digit9),
    (WinitKeyCode::Backspace, KeyCode::Backspace),
    (WinitKeyCode::Delete, KeyCode::Delete),
    (WinitKeyCode::Enter, KeyCode::Enter),
    (WinitKeyCode::Tab, KeyCode::Tab),
    (WinitKeyCode::Space, KeyCode::Space),
    (WinitKeyCode::Escape, KeyCode::Escape),
    (WinitKeyCode::ArrowUp, KeyCode::ArrowUp),
    (WinitKeyCode::ArrowDown, KeyCode::ArrowDown),
    (WinitKeyCode::ArrowLeft, KeyCode::ArrowLeft),
    (WinitKeyCode::ArrowRight, KeyCode::ArrowRight),
    (WinitKeyCode::Home, KeyCode::Home),
    (WinitKeyCode::End, KeyCode::End),
    (WinitKeyCode::NumpadEnter, KeyCode::Enter),
];

pub(crate) fn app_key_code(physical_key: PhysicalKey, logical_key: &Key) -> KeyCode {
    let named = match logical_key {
        Key::Named(named) => NAMED_KEYS
            .iter()
            .find(|(key, _)| key == named)
            .map(|(_, code)| *code),
        _ => None,
    };
    named.unwrap_or_else(|| match physical_key {
        PhysicalKey::Code(physical) => PHYSICAL_KEYS
            .iter()
            .find(|(key, _)| *key == physical)
            .map_or(KeyCode::Unknown, |(_, code)| *code),
        _ => KeyCode::Unknown,
    })
}

pub(crate) fn app_modifiers(
    current_modifiers: winit::keyboard::ModifiersState,
) -> cranpose_app_shell::Modifiers {
    cranpose_app_shell::Modifiers {
        shift: current_modifiers.contains(winit::keyboard::ModifiersState::SHIFT),
        ctrl: current_modifiers.contains(winit::keyboard::ModifiersState::CONTROL),
        alt: current_modifiers.contains(winit::keyboard::ModifiersState::ALT),
        meta: current_modifiers.contains(winit::keyboard::ModifiersState::META),
    }
}

#[cfg(test)]
#[path = "tests/desktop_input.rs"]
mod tests;
