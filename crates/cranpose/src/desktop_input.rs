//! Winit → framework input translation shared by every desktop shell
//! (the wgpu shell and the slim Vulkan shell).

use cranpose_app_shell::AppShell;

pub(crate) fn dispatch_keyboard_input<R>(
    app: &mut AppShell<R>,
    current_modifiers: winit::keyboard::ModifiersState,
    event: winit::event::KeyEvent,
) where
    R: cranpose_render_common::Renderer,
    R::Error: std::fmt::Debug,
{
    use cranpose_app_shell::{KeyEvent, KeyEventType};
    use winit::event::ElementState;
    use winit::keyboard::Key;

    let event_type = match event.state {
        ElementState::Pressed => KeyEventType::KeyDown,
        ElementState::Released => KeyEventType::KeyUp,
    };
    let text = match &event.logical_key {
        Key::Character(s) => s.to_string(),
        _ => String::new(),
    };
    let key_code = app_key_code(event.physical_key);
    let key_event = KeyEvent::new(key_code, text, app_modifiers(current_modifiers), event_type);

    if key_code == cranpose_app_shell::KeyCode::D && event_type == KeyEventType::KeyDown {
        app.log_debug_info();
    }

    app.on_key_event(&key_event);
}

pub(crate) fn app_key_code(
    physical_key: winit::keyboard::PhysicalKey,
) -> cranpose_app_shell::KeyCode {
    use cranpose_app_shell::KeyCode;
    use winit::keyboard::PhysicalKey;

    match physical_key {
        PhysicalKey::Code(code) => match code {
            winit::keyboard::KeyCode::KeyA => KeyCode::A,
            winit::keyboard::KeyCode::KeyB => KeyCode::B,
            winit::keyboard::KeyCode::KeyC => KeyCode::C,
            winit::keyboard::KeyCode::KeyD => KeyCode::D,
            winit::keyboard::KeyCode::KeyE => KeyCode::E,
            winit::keyboard::KeyCode::KeyF => KeyCode::F,
            winit::keyboard::KeyCode::KeyG => KeyCode::G,
            winit::keyboard::KeyCode::KeyH => KeyCode::H,
            winit::keyboard::KeyCode::KeyI => KeyCode::I,
            winit::keyboard::KeyCode::KeyJ => KeyCode::J,
            winit::keyboard::KeyCode::KeyK => KeyCode::K,
            winit::keyboard::KeyCode::KeyL => KeyCode::L,
            winit::keyboard::KeyCode::KeyM => KeyCode::M,
            winit::keyboard::KeyCode::KeyN => KeyCode::N,
            winit::keyboard::KeyCode::KeyO => KeyCode::O,
            winit::keyboard::KeyCode::KeyP => KeyCode::P,
            winit::keyboard::KeyCode::KeyQ => KeyCode::Q,
            winit::keyboard::KeyCode::KeyR => KeyCode::R,
            winit::keyboard::KeyCode::KeyS => KeyCode::S,
            winit::keyboard::KeyCode::KeyT => KeyCode::T,
            winit::keyboard::KeyCode::KeyU => KeyCode::U,
            winit::keyboard::KeyCode::KeyV => KeyCode::V,
            winit::keyboard::KeyCode::KeyW => KeyCode::W,
            winit::keyboard::KeyCode::KeyX => KeyCode::X,
            winit::keyboard::KeyCode::KeyY => KeyCode::Y,
            winit::keyboard::KeyCode::KeyZ => KeyCode::Z,
            winit::keyboard::KeyCode::Digit0 => KeyCode::Digit0,
            winit::keyboard::KeyCode::Digit1 => KeyCode::Digit1,
            winit::keyboard::KeyCode::Digit2 => KeyCode::Digit2,
            winit::keyboard::KeyCode::Digit3 => KeyCode::Digit3,
            winit::keyboard::KeyCode::Digit4 => KeyCode::Digit4,
            winit::keyboard::KeyCode::Digit5 => KeyCode::Digit5,
            winit::keyboard::KeyCode::Digit6 => KeyCode::Digit6,
            winit::keyboard::KeyCode::Digit7 => KeyCode::Digit7,
            winit::keyboard::KeyCode::Digit8 => KeyCode::Digit8,
            winit::keyboard::KeyCode::Digit9 => KeyCode::Digit9,
            winit::keyboard::KeyCode::Backspace => KeyCode::Backspace,
            winit::keyboard::KeyCode::Delete => KeyCode::Delete,
            winit::keyboard::KeyCode::Enter => KeyCode::Enter,
            winit::keyboard::KeyCode::Tab => KeyCode::Tab,
            winit::keyboard::KeyCode::Space => KeyCode::Space,
            winit::keyboard::KeyCode::Escape => KeyCode::Escape,
            winit::keyboard::KeyCode::ArrowUp => KeyCode::ArrowUp,
            winit::keyboard::KeyCode::ArrowDown => KeyCode::ArrowDown,
            winit::keyboard::KeyCode::ArrowLeft => KeyCode::ArrowLeft,
            winit::keyboard::KeyCode::ArrowRight => KeyCode::ArrowRight,
            winit::keyboard::KeyCode::Home => KeyCode::Home,
            winit::keyboard::KeyCode::End => KeyCode::End,
            _ => KeyCode::Unknown,
        },
        _ => KeyCode::Unknown,
    }
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
