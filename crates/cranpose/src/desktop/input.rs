use cranpose_app_shell::{Key, SurfaceMut};
use winit::keyboard::{Key as LogicalKey, KeyCode, NamedKey, PhysicalKey};

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
        LogicalKey::Character(s) => s.to_string(),
        _ => String::new(),
    };
    let key = app_key(event.physical_key, &event.logical_key);
    let key_event = KeyEvent::new(key, text, app_modifiers(current_modifiers), event_type)
        .with_repeat_count(u32::from(event.repeat));
    surface.on_key_event(&key_event);
}

const NAMED_KEYS: [(NamedKey, Key); 45] = [
    (NamedKey::ArrowUp, Key::DirectionUp),
    (NamedKey::ArrowDown, Key::DirectionDown),
    (NamedKey::ArrowLeft, Key::DirectionLeft),
    (NamedKey::ArrowRight, Key::DirectionRight),
    (NamedKey::Home, Key::MoveHome),
    (NamedKey::End, Key::MoveEnd),
    (NamedKey::PageUp, Key::PageUp),
    (NamedKey::PageDown, Key::PageDown),
    (NamedKey::Enter, Key::Enter),
    (NamedKey::Tab, Key::Tab),
    (NamedKey::Escape, Key::Escape),
    (NamedKey::Backspace, Key::Backspace),
    (NamedKey::Delete, Key::Delete),
    (NamedKey::F1, Key::F1),
    (NamedKey::F2, Key::F2),
    (NamedKey::F3, Key::F3),
    (NamedKey::F4, Key::F4),
    (NamedKey::F5, Key::F5),
    (NamedKey::F6, Key::F6),
    (NamedKey::F7, Key::F7),
    (NamedKey::F8, Key::F8),
    (NamedKey::F9, Key::F9),
    (NamedKey::F10, Key::F10),
    (NamedKey::F11, Key::F11),
    (NamedKey::F12, Key::F12),
    (NamedKey::ContextMenu, Key::Menu),
    (NamedKey::BrowserSearch, Key::Search),
    (NamedKey::MediaPlay, Key::MediaPlay),
    (NamedKey::MediaPause, Key::MediaPause),
    (NamedKey::MediaPlayPause, Key::MediaPlayPause),
    (NamedKey::MediaStop, Key::MediaStop),
    (NamedKey::MediaTrackNext, Key::MediaNext),
    (NamedKey::MediaTrackPrevious, Key::MediaPrevious),
    (NamedKey::MediaRewind, Key::MediaRewind),
    (NamedKey::MediaFastForward, Key::MediaFastForward),
    (NamedKey::MediaRecord, Key::MediaRecord),
    (NamedKey::ChannelUp, Key::ChannelUp),
    (NamedKey::ChannelDown, Key::ChannelDown),
    (NamedKey::Info, Key::Info),
    (NamedKey::Guide, Key::Guide),
    (NamedKey::Settings, Key::Settings),
    (NamedKey::ColorF0Red, Key::ProgramRed),
    (NamedKey::ColorF1Green, Key::ProgramGreen),
    (NamedKey::ColorF2Yellow, Key::ProgramYellow),
    (NamedKey::ColorF3Blue, Key::ProgramBlue),
];

const PHYSICAL_KEYS: [(KeyCode, Key); 76] = [
    (KeyCode::KeyA, Key::A),
    (KeyCode::KeyB, Key::B),
    (KeyCode::KeyC, Key::C),
    (KeyCode::KeyD, Key::D),
    (KeyCode::KeyE, Key::E),
    (KeyCode::KeyF, Key::F),
    (KeyCode::KeyG, Key::G),
    (KeyCode::KeyH, Key::H),
    (KeyCode::KeyI, Key::I),
    (KeyCode::KeyJ, Key::J),
    (KeyCode::KeyK, Key::K),
    (KeyCode::KeyL, Key::L),
    (KeyCode::KeyM, Key::M),
    (KeyCode::KeyN, Key::N),
    (KeyCode::KeyO, Key::O),
    (KeyCode::KeyP, Key::P),
    (KeyCode::KeyQ, Key::Q),
    (KeyCode::KeyR, Key::R),
    (KeyCode::KeyS, Key::S),
    (KeyCode::KeyT, Key::T),
    (KeyCode::KeyU, Key::U),
    (KeyCode::KeyV, Key::V),
    (KeyCode::KeyW, Key::W),
    (KeyCode::KeyX, Key::X),
    (KeyCode::KeyY, Key::Y),
    (KeyCode::KeyZ, Key::Z),
    (KeyCode::Digit0, Key::Zero),
    (KeyCode::Digit1, Key::One),
    (KeyCode::Digit2, Key::Two),
    (KeyCode::Digit3, Key::Three),
    (KeyCode::Digit4, Key::Four),
    (KeyCode::Digit5, Key::Five),
    (KeyCode::Digit6, Key::Six),
    (KeyCode::Digit7, Key::Seven),
    (KeyCode::Digit8, Key::Eight),
    (KeyCode::Digit9, Key::Nine),
    (KeyCode::Backspace, Key::Backspace),
    (KeyCode::Delete, Key::Delete),
    (KeyCode::Enter, Key::Enter),
    (KeyCode::Tab, Key::Tab),
    (KeyCode::Space, Key::Spacebar),
    (KeyCode::Escape, Key::Escape),
    (KeyCode::ArrowUp, Key::DirectionUp),
    (KeyCode::ArrowDown, Key::DirectionDown),
    (KeyCode::ArrowLeft, Key::DirectionLeft),
    (KeyCode::ArrowRight, Key::DirectionRight),
    (KeyCode::Home, Key::MoveHome),
    (KeyCode::End, Key::MoveEnd),
    (KeyCode::PageUp, Key::PageUp),
    (KeyCode::PageDown, Key::PageDown),
    (KeyCode::ShiftLeft, Key::ShiftLeft),
    (KeyCode::ShiftRight, Key::ShiftRight),
    (KeyCode::ControlLeft, Key::CtrlLeft),
    (KeyCode::ControlRight, Key::CtrlRight),
    (KeyCode::AltLeft, Key::AltLeft),
    (KeyCode::AltRight, Key::AltRight),
    (KeyCode::MetaLeft, Key::MetaLeft),
    (KeyCode::MetaRight, Key::MetaRight),
    (KeyCode::Minus, Key::Minus),
    (KeyCode::Equal, Key::Equals),
    (KeyCode::BracketLeft, Key::LeftBracket),
    (KeyCode::BracketRight, Key::RightBracket),
    (KeyCode::Backslash, Key::Backslash),
    (KeyCode::Semicolon, Key::Semicolon),
    (KeyCode::Quote, Key::Apostrophe),
    (KeyCode::Comma, Key::Comma),
    (KeyCode::Period, Key::Period),
    (KeyCode::Slash, Key::Slash),
    (KeyCode::Backquote, Key::Grave),
    (KeyCode::ContextMenu, Key::Menu),
    (KeyCode::MediaPlayPause, Key::MediaPlayPause),
    (KeyCode::MediaStop, Key::MediaStop),
    (KeyCode::MediaTrackNext, Key::MediaNext),
    (KeyCode::MediaTrackPrevious, Key::MediaPrevious),
    (KeyCode::BrowserSearch, Key::Search),
    (KeyCode::NumpadEnter, Key::NumPadEnter),
];

// The keypad Enter is NumPadEnter; the keypad's other keys follow their
// meaning with Num Lock (an arrow, Home), and the rest keep their position.
pub(crate) fn app_key(physical_key: PhysicalKey, logical_key: &LogicalKey) -> Key {
    if physical_key == PhysicalKey::Code(KeyCode::NumpadEnter) {
        return Key::NumPadEnter;
    }
    let named = match logical_key {
        LogicalKey::Named(named) => NAMED_KEYS
            .iter()
            .find(|(key, _)| key == named)
            .map(|(_, key)| *key),
        _ => None,
    };
    named.unwrap_or_else(|| match physical_key {
        PhysicalKey::Code(physical) => PHYSICAL_KEYS
            .iter()
            .find(|(code, _)| *code == physical)
            .map_or(Key::Unknown, |(_, key)| *key),
        PhysicalKey::Unidentified(_) => Key::Unknown,
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
#[path = "tests/input.rs"]
mod tests;
