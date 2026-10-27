fn web_key_modifiers(event: &web_sys::KeyboardEvent) -> cranpose_app_shell::Modifiers {
    cranpose_app_shell::Modifiers {
        shift: event.shift_key(),
        ctrl: event.ctrl_key(),
        alt: event.alt_key(),
        meta: event.meta_key(),
    }
}

pub(super) fn web_key_event_prefix(
    event: &web_sys::KeyboardEvent,
) -> Option<(cranpose_app_shell::Key, cranpose_app_shell::Modifiers)> {
    if event.is_composing() || event.key_code() == 229 {
        return None;
    }
    Some((
        cranpose_app_shell::Key::from_dom_code(&event.code()),
        web_key_modifiers(event),
    ))
}
